use agent_local::DiscoveryDescriptor;
use anyhow::{Context, Result, anyhow, bail};
use tokio_util::sync::CancellationToken;

use super::running::RunningEpoch;
use super::supervisor::{CutoverOutcome, EpochSupervisor};
use crate::model_setup::PreparedRuntime;

pub(super) enum GuardedCutover {
  Outcome(CutoverOutcome),
  Shutdown,
}

enum TransitionGuard {
  Continue,
  Shutdown,
  OldExited,
}

impl EpochSupervisor {
  pub async fn cutover(&mut self, prepared: PreparedRuntime) -> Result<CutoverOutcome> {
    match self.cutover_guarded(prepared, None).await? {
      GuardedCutover::Outcome(outcome) => Ok(outcome),
      GuardedCutover::Shutdown => Ok(CutoverOutcome::KeptCurrent),
    }
  }

  pub(super) async fn cutover_guarded(
    &mut self,
    prepared: PreparedRuntime,
    shutdown: Option<&CancellationToken>,
  ) -> Result<GuardedCutover> {
    let old_descriptor = self
      .active
      .as_ref()
      .map(|epoch| epoch.descriptor.clone())
      .ok_or_else(|| anyhow!("agentd epoch supervisor is not running"))?;
    let operation_id = prepared.activation_operation_id().map(str::to_owned);
    match self.guard(shutdown).await {
      TransitionGuard::Continue => {}
      TransitionGuard::Shutdown => {
        crate::runtime_cleanup::dispose_prepared(prepared, self.stop_timeout).await;
        return Ok(GuardedCutover::Shutdown);
      }
      TransitionGuard::OldExited => {
        crate::runtime_cleanup::dispose_prepared(prepared, self.stop_timeout).await;
        self.stop_active().await;
        bail!("current agentd epoch exited before replacement startup");
      }
    }
    let replacement = match self.start_unpublished(prepared).await {
      Ok(replacement) => replacement,
      Err(_) => {
        self
          .acknowledge_failure(operation_id.as_deref(), "epoch_start_failed")
          .await;
        match self.guard(shutdown).await {
          TransitionGuard::Shutdown => return Ok(GuardedCutover::Shutdown),
          TransitionGuard::OldExited => {
            self.stop_active().await;
            bail!("current agentd epoch exited during replacement startup");
          }
          TransitionGuard::Continue => {}
        }
        tracing::error!(
          error_code = "epoch_start_failed",
          "replacement epoch failed to start; retaining current epoch"
        );
        return Ok(GuardedCutover::Outcome(CutoverOutcome::KeptCurrent));
      }
    };
    match self.guard(shutdown).await {
      TransitionGuard::Continue => {}
      TransitionGuard::Shutdown => {
        replacement.stop().await;
        return Ok(GuardedCutover::Shutdown);
      }
      TransitionGuard::OldExited => {
        replacement.stop().await;
        self
          .acknowledge_failure(operation_id.as_deref(), "epoch_current_exited")
          .await;
        self.stop_active().await;
        bail!("current agentd epoch exited before replacement publication");
      }
    }
    if Self::candidate_exited(&replacement).await {
      replacement.stop().await;
      self
        .acknowledge_failure(operation_id.as_deref(), "epoch_replacement_exited")
        .await;
      return Ok(GuardedCutover::Outcome(CutoverOutcome::KeptCurrent));
    }
    self
      .publish_and_finish(replacement, old_descriptor, operation_id, shutdown)
      .await
  }

  async fn publish_and_finish(
    &mut self,
    replacement: RunningEpoch,
    old: DiscoveryDescriptor,
    operation_id: Option<String>,
    shutdown: Option<&CancellationToken>,
  ) -> Result<GuardedCutover> {
    if self.publisher.publish(&replacement.descriptor).is_err() {
      let recovery = self.rollback_verified(&old, &replacement.descriptor);
      replacement.stop().await;
      self
        .acknowledge_failure(operation_id.as_deref(), "epoch_publication_failed")
        .await;
      return match recovery {
        Ok(()) => {
          tracing::error!(
            error_code = "epoch_publication_failed",
            "replacement epoch publication failed; retaining current epoch"
          );
          Ok(GuardedCutover::Outcome(CutoverOutcome::KeptCurrent))
        }
        Err(error) => {
          self.stop_active().await;
          Err(error).context("failed closed after ambiguous epoch publication")
        }
      };
    }
    if Self::candidate_exited(&replacement).await {
      return self
        .rollback_exited_candidate(replacement, old, operation_id.as_deref())
        .await;
    }
    match self.guard(shutdown).await {
      TransitionGuard::Continue => {}
      guard => return self.rollback_guarded(replacement, old, guard).await,
    }
    if Self::candidate_exited(&replacement).await {
      return self
        .rollback_exited_candidate(replacement, old, operation_id.as_deref())
        .await;
    }
    if let Some(id) = operation_id.as_deref()
      && !self.coordinator.activation_succeeded(id).await
    {
      let rollback = self.rollback_verified(&old, &replacement.descriptor);
      replacement.stop().await;
      if let Err(error) = rollback {
        self.stop_active().await;
        return Err(error).context("failed closed after stale activation rollback");
      }
      tracing::error!(
        error_code = "epoch_activation_ack_rejected",
        "replacement epoch activation acknowledgement was rejected"
      );
      return Ok(GuardedCutover::Outcome(CutoverOutcome::KeptCurrent));
    }
    self.pause_after_activation_ack().await;
    if Self::candidate_exited(&replacement).await {
      return self
        .rollback_exited_after_ack(replacement, old, operation_id.as_deref())
        .await;
    }
    match self.guard(shutdown).await {
      TransitionGuard::Continue => {}
      guard => return self.rollback_guarded(replacement, old, guard).await,
    }
    if Self::candidate_exited(&replacement).await {
      return self
        .rollback_exited_after_ack(replacement, old, operation_id.as_deref())
        .await;
    }
    let previous = self
      .active
      .replace(replacement)
      .expect("active epoch checked");
    previous.stop().await;
    Ok(GuardedCutover::Outcome(CutoverOutcome::Switched))
  }

  async fn rollback_guarded(
    &mut self,
    replacement: RunningEpoch,
    old: DiscoveryDescriptor,
    guard: TransitionGuard,
  ) -> Result<GuardedCutover> {
    let rollback = self.rollback_verified(&old, &replacement.descriptor);
    replacement.stop().await;
    if let Err(error) = rollback {
      self.stop_active().await;
      return Err(error).context("failed closed after interrupted epoch publication");
    }
    match guard {
      TransitionGuard::Shutdown => Ok(GuardedCutover::Shutdown),
      TransitionGuard::OldExited => {
        self.stop_active().await;
        bail!("current agentd epoch exited during replacement publication")
      }
      TransitionGuard::Continue => unreachable!(),
    }
  }

  async fn rollback_exited_candidate(
    &mut self,
    replacement: RunningEpoch,
    old: DiscoveryDescriptor,
    operation_id: Option<&str>,
  ) -> Result<GuardedCutover> {
    let rollback = self.rollback_verified(&old, &replacement.descriptor);
    replacement.stop().await;
    self
      .acknowledge_failure(operation_id, "epoch_replacement_exited")
      .await;
    if let Err(error) = rollback {
      self.stop_active().await;
      return Err(error).context("failed closed after replacement runtime exit");
    }
    Ok(GuardedCutover::Outcome(CutoverOutcome::KeptCurrent))
  }

  async fn rollback_exited_after_ack(
    &mut self,
    replacement: RunningEpoch,
    old: DiscoveryDescriptor,
    operation_id: Option<&str>,
  ) -> Result<GuardedCutover> {
    let rollback = self.rollback_verified(&old, &replacement.descriptor);
    replacement.stop().await;
    if let Some(operation_id) = operation_id
      && !self
        .coordinator
        .activation_failed_after_success(operation_id, "epoch_replacement_exited")
        .await
    {
      tracing::error!(
        error_code = "epoch_activation_compensation_rejected",
        "replacement epoch activation compensation was rejected"
      );
    }
    if let Err(error) = rollback {
      self.stop_active().await;
      return Err(error).context("failed closed after acknowledged replacement exit");
    }
    Ok(GuardedCutover::Outcome(CutoverOutcome::KeptCurrent))
  }

  async fn candidate_exited(candidate: &RunningEpoch) -> bool {
    tokio::task::yield_now().await;
    candidate.has_exited()
  }

  async fn guard(&self, shutdown: Option<&CancellationToken>) -> TransitionGuard {
    tokio::task::yield_now().await;
    if shutdown.is_some_and(CancellationToken::is_cancelled) {
      TransitionGuard::Shutdown
    } else if self.active.as_ref().is_some_and(RunningEpoch::has_exited) {
      TransitionGuard::OldExited
    } else {
      TransitionGuard::Continue
    }
  }

  fn rollback_verified(
    &self,
    old: &DiscoveryDescriptor,
    candidate: &DiscoveryDescriptor,
  ) -> Result<()> {
    let observed = self.publisher.read()?;
    if observed == *old {
      return Ok(());
    }
    if observed != *candidate {
      bail!("discovery descriptor has an unexpected identity");
    }
    let rollback = self.publisher.publish(old);
    if self.publisher.read()? == *old {
      Ok(())
    } else {
      match rollback {
        Ok(()) => bail!("old discovery descriptor rollback could not be verified"),
        Err(error) => Err(error).context("old discovery descriptor rollback failed"),
      }
    }
  }

  async fn acknowledge_failure(&self, operation_id: Option<&str>, code: &'static str) {
    if let Some(operation_id) = operation_id
      && !self.coordinator.activation_failed(operation_id, code).await
    {
      tracing::error!(
        error_code = "epoch_activation_ack_rejected",
        "replacement epoch failure acknowledgement was rejected"
      );
    }
  }
}
