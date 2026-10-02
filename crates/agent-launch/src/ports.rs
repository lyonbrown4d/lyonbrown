use crate::{Handshake, ProbeError, SpawnError, SpawnMonitor, SpawnRequest};
use agent_local::{DiscoveryDescriptor, DiscoveryError};
use async_trait::async_trait;

pub trait StartupGuard: Send {}

pub trait DiscoverySource: Send + Sync {
  fn read(&self) -> Result<Option<DiscoveryDescriptor>, DiscoveryError>;

  fn try_acquire_startup_lock(&self) -> Result<Option<Box<dyn StartupGuard>>, DiscoveryError>;
}

#[async_trait]
pub trait DaemonSpawner: Send + Sync {
  async fn spawn(&self, request: &SpawnRequest) -> Result<SpawnMonitor, SpawnError>;
}

#[async_trait]
pub trait ReadinessProbe: Send + Sync {
  async fn probe(&self, descriptor: &DiscoveryDescriptor) -> Result<Handshake, ProbeError>;
}
