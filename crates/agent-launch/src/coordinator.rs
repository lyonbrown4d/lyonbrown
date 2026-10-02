use crate::{
  DaemonSpawner, DaemonTarget, DiscoverySource, Handshake, LaunchError, LaunchRequest, ProbeError,
  ReadinessProbe, SpawnRequest,
};
use agent_local::{DiscoveryDescriptor, ScopeFingerprint};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub struct LaunchCoordinator {
  discovery: Arc<dyn DiscoverySource>,
  spawner: Arc<dyn DaemonSpawner>,
  probe: Arc<dyn ReadinessProbe>,
  poll_interval: Duration,
}

impl LaunchCoordinator {
  pub fn new(
    discovery: Arc<dyn DiscoverySource>,
    spawner: Arc<dyn DaemonSpawner>,
    probe: Arc<dyn ReadinessProbe>,
  ) -> Self {
    Self {
      discovery,
      spawner,
      probe,
      poll_interval: DEFAULT_POLL_INTERVAL,
    }
  }

  pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
    if !poll_interval.is_zero() {
      self.poll_interval = poll_interval;
    }
    self
  }

  pub async fn connect_or_start(
    &self,
    request: &LaunchRequest,
  ) -> Result<DaemonTarget, LaunchError> {
    let deadline = Instant::now() + request.timeout();
    if let Some(descriptor) = self.discovery.read()?
      && let Some(target) = self.probe_existing(request, descriptor).await?
    {
      return Ok(target);
    }
    if !request.auto_start() {
      return Err(LaunchError::AutoStartDisabled);
    }

    let _startup_guard = self
      .acquire_startup_lock(deadline, request.timeout())
      .await?;
    if let Some(descriptor) = self.discovery.read()?
      && let Some(target) = self.probe_existing(request, descriptor).await?
    {
      return Ok(target);
    }

    let mut monitor = self
      .spawner
      .spawn(&SpawnRequest::from_launch_request(request))
      .await?;
    self.wait_until_ready(request, deadline, &mut monitor).await
  }

  async fn acquire_startup_lock(
    &self,
    deadline: Instant,
    timeout: Duration,
  ) -> Result<Box<dyn crate::StartupGuard>, LaunchError> {
    loop {
      if let Some(guard) = self.discovery.try_acquire_startup_lock()? {
        return Ok(guard);
      }
      if Instant::now() >= deadline {
        return Err(LaunchError::StartupLockTimeout { timeout });
      }
      self.sleep_until_next_poll(deadline).await;
    }
  }

  async fn wait_until_ready(
    &self,
    request: &LaunchRequest,
    deadline: Instant,
    monitor: &mut crate::SpawnMonitor,
  ) -> Result<DaemonTarget, LaunchError> {
    loop {
      if Instant::now() >= deadline {
        return Err(LaunchError::ReadinessTimeout {
          timeout: request.timeout(),
        });
      }
      if let Some(descriptor) = self.discovery.read()?
        && let Some(target) = self.probe_existing(request, descriptor).await?
      {
        return Ok(target);
      }
      tokio::select! {
        exit = monitor.wait() => {
          return Err(LaunchError::DaemonExited {
            exit_code: exit.exit_code(),
            stderr: exit.summary(1_024),
          });
        }
        _ = self.sleep_until_next_poll(deadline) => {}
      }
    }
  }

  async fn probe_existing(
    &self,
    request: &LaunchRequest,
    descriptor: DiscoveryDescriptor,
  ) -> Result<Option<DaemonTarget>, LaunchError> {
    let target = self.probe_target(descriptor).await?;
    if let Some(target) = target {
      self.validate_scope(request.scope_fingerprint(), target.descriptor())?;
      Ok(Some(target))
    } else {
      Ok(None)
    }
  }

  async fn probe_target(
    &self,
    descriptor: DiscoveryDescriptor,
  ) -> Result<Option<DaemonTarget>, LaunchError> {
    let handshake = match self.probe.probe(&descriptor).await {
      Ok(handshake) => handshake,
      Err(ProbeError::Unavailable) => return Ok(None),
      Err(ProbeError::AuthenticationRejected) => {
        return Err(LaunchError::AuthenticationRejected);
      }
      Err(ProbeError::HandshakeMismatch { reason }) => {
        return Err(LaunchError::HandshakeMismatch { reason });
      }
    };
    self.validate_handshake(&descriptor, &handshake)?;
    Ok(Some(DaemonTarget::new(descriptor)))
  }

  fn validate_scope(
    &self,
    expected: &ScopeFingerprint,
    descriptor: &DiscoveryDescriptor,
  ) -> Result<(), LaunchError> {
    if descriptor.scope_fingerprint() == expected {
      return Ok(());
    }
    Err(LaunchError::ScopeMismatch {
      expected: expected.as_str().to_owned(),
      actual: descriptor.scope_fingerprint().as_str().to_owned(),
    })
  }

  fn validate_handshake(
    &self,
    descriptor: &DiscoveryDescriptor,
    handshake: &Handshake,
  ) -> Result<(), LaunchError> {
    if handshake.server_instance_id() != descriptor.server_instance_id() {
      return Err(LaunchError::HandshakeMismatch {
        reason: "server instance id differs from discovery descriptor",
      });
    }
    if handshake.scope_fingerprint() != descriptor.scope_fingerprint() {
      return Err(LaunchError::HandshakeMismatch {
        reason: "scope differs from discovery descriptor",
      });
    }
    Ok(())
  }

  async fn sleep_until_next_poll(&self, deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    tokio::time::sleep(self.poll_interval.min(remaining)).await;
  }
}
