use std::sync::Arc;

use agent_local::{DiscoveryDescriptor, DiscoveryStore};
use anyhow::{Context, Result};

pub trait DescriptorPublisher: Send + Sync + 'static {
  fn publish(&self, descriptor: &DiscoveryDescriptor) -> Result<()>;
  fn read(&self) -> Result<DiscoveryDescriptor>;
}

pub struct StoreDescriptorPublisher {
  store: DiscoveryStore,
}

impl StoreDescriptorPublisher {
  pub fn new(store: DiscoveryStore) -> Arc<Self> {
    Arc::new(Self { store })
  }
}

impl DescriptorPublisher for StoreDescriptorPublisher {
  fn publish(&self, descriptor: &DiscoveryDescriptor) -> Result<()> {
    self
      .store
      .publish(descriptor)
      .context("failed to publish agentd discovery")
  }

  fn read(&self) -> Result<DiscoveryDescriptor> {
    self.store.read().context("failed to read agentd discovery")
  }
}
