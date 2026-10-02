#![allow(dead_code)]

use agent_launch::{
  DaemonExit, DaemonSpawner, DiscoverySource, Handshake, ProbeError, ReadinessProbe, SpawnError,
  SpawnMonitor, SpawnRequest, StartupGuard,
};
use agent_local::{DiscoveryDescriptor, DiscoveryError, ScopeFingerprint, SecretToken};
use async_trait::async_trait;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

pub fn workspace() -> PathBuf {
  std::env::current_dir()
    .expect("current directory")
    .canonicalize()
    .expect("canonical workspace")
}

pub fn descriptor(scope: &ScopeFingerprint, port: u16) -> DiscoveryDescriptor {
  DiscoveryDescriptor::new(
    SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
    u32::from(port),
    Uuid::new_v4(),
    SecretToken::generate().expect("test token"),
    scope.clone(),
  )
  .expect("test descriptor")
}

#[derive(Clone, Default)]
pub struct FakeDiscovery {
  descriptor: Arc<Mutex<Option<DiscoveryDescriptor>>>,
  lock: Arc<AsyncMutex<()>>,
  lock_held: Arc<AtomicBool>,
  lock_attempts: Arc<AtomicUsize>,
}

impl FakeDiscovery {
  pub fn with_descriptor(descriptor: DiscoveryDescriptor) -> Self {
    let source = Self::default();
    source.replace(Some(descriptor));
    source
  }

  pub fn replace(&self, descriptor: Option<DiscoveryDescriptor>) {
    *self.descriptor.lock().expect("descriptor mutex") = descriptor;
  }

  pub fn lock_held(&self) -> bool {
    self.lock_held.load(Ordering::SeqCst)
  }

  pub fn lock_attempts(&self) -> usize {
    self.lock_attempts.load(Ordering::SeqCst)
  }
}

struct FakeGuard {
  _guard: OwnedMutexGuard<()>,
  lock_held: Arc<AtomicBool>,
}

impl StartupGuard for FakeGuard {}

impl Drop for FakeGuard {
  fn drop(&mut self) {
    self.lock_held.store(false, Ordering::SeqCst);
  }
}

impl DiscoverySource for FakeDiscovery {
  fn read(&self) -> Result<Option<DiscoveryDescriptor>, DiscoveryError> {
    Ok(self.descriptor.lock().expect("descriptor mutex").clone())
  }

  fn try_acquire_startup_lock(&self) -> Result<Option<Box<dyn StartupGuard>>, DiscoveryError> {
    self.lock_attempts.fetch_add(1, Ordering::SeqCst);
    let Ok(guard) = self.lock.clone().try_lock_owned() else {
      return Ok(None);
    };
    self.lock_held.store(true, Ordering::SeqCst);
    Ok(Some(Box::new(FakeGuard {
      _guard: guard,
      lock_held: Arc::clone(&self.lock_held),
    })))
  }
}

#[derive(Clone)]
pub struct FakeSpawner {
  source: FakeDiscovery,
  descriptor: Option<DiscoveryDescriptor>,
  exit: Option<DaemonExit>,
  spawn_count: Arc<AtomicUsize>,
  spawned_under_lock: Arc<AtomicBool>,
}

impl FakeSpawner {
  pub fn publishing(source: FakeDiscovery, descriptor: DiscoveryDescriptor) -> Self {
    Self::new(source, Some(descriptor), None)
  }

  pub fn silent(source: FakeDiscovery) -> Self {
    Self::new(source, None, None)
  }

  pub fn exiting(source: FakeDiscovery, exit: DaemonExit) -> Self {
    Self::new(source, None, Some(exit))
  }

  fn new(
    source: FakeDiscovery,
    descriptor: Option<DiscoveryDescriptor>,
    exit: Option<DaemonExit>,
  ) -> Self {
    Self {
      source,
      descriptor,
      exit,
      spawn_count: Arc::new(AtomicUsize::new(0)),
      spawned_under_lock: Arc::new(AtomicBool::new(false)),
    }
  }

  pub fn spawn_count(&self) -> usize {
    self.spawn_count.load(Ordering::SeqCst)
  }

  pub fn spawned_under_lock(&self) -> bool {
    self.spawned_under_lock.load(Ordering::SeqCst)
  }
}

#[async_trait]
impl DaemonSpawner for FakeSpawner {
  async fn spawn(&self, request: &SpawnRequest) -> Result<SpawnMonitor, SpawnError> {
    assert_eq!(request.canonical_workspace(), workspace());
    self.spawn_count.fetch_add(1, Ordering::SeqCst);
    self
      .spawned_under_lock
      .store(self.source.lock_held(), Ordering::SeqCst);
    if let Some(descriptor) = &self.descriptor {
      self.source.replace(Some(descriptor.clone()));
    }
    Ok(match &self.exit {
      Some(exit) => SpawnMonitor::completed(exit.clone()),
      None => SpawnMonitor::detached(),
    })
  }
}

#[derive(Clone, Default)]
pub struct FakeProbe {
  outcomes: Arc<Mutex<HashMap<Uuid, ProbeOutcome>>>,
  observations: Arc<Mutex<Vec<(Uuid, bool)>>>,
  source: Option<FakeDiscovery>,
}

#[derive(Clone)]
enum ProbeOutcome {
  Ready,
  Unavailable,
  AuthenticationRejected,
  WrongInstance,
}

impl FakeProbe {
  pub fn observing(source: FakeDiscovery) -> Self {
    Self {
      source: Some(source),
      ..Self::default()
    }
  }

  pub fn ready(&self, descriptor: &DiscoveryDescriptor) {
    self.set(descriptor, ProbeOutcome::Ready);
  }

  pub fn unavailable(&self, descriptor: &DiscoveryDescriptor) {
    self.set(descriptor, ProbeOutcome::Unavailable);
  }

  pub fn authentication_rejected(&self, descriptor: &DiscoveryDescriptor) {
    self.set(descriptor, ProbeOutcome::AuthenticationRejected);
  }

  pub fn wrong_instance(&self, descriptor: &DiscoveryDescriptor) {
    self.set(descriptor, ProbeOutcome::WrongInstance);
  }

  fn set(&self, descriptor: &DiscoveryDescriptor, outcome: ProbeOutcome) {
    self
      .outcomes
      .lock()
      .expect("outcomes mutex")
      .insert(descriptor.server_instance_id(), outcome);
  }

  pub fn observations(&self) -> Vec<(Uuid, bool)> {
    self
      .observations
      .lock()
      .expect("observations mutex")
      .clone()
  }
}

#[async_trait]
impl ReadinessProbe for FakeProbe {
  async fn probe(&self, descriptor: &DiscoveryDescriptor) -> Result<Handshake, ProbeError> {
    let under_lock = self.source.as_ref().is_some_and(FakeDiscovery::lock_held);
    self
      .observations
      .lock()
      .expect("observations mutex")
      .push((descriptor.server_instance_id(), under_lock));
    match self
      .outcomes
      .lock()
      .expect("outcomes mutex")
      .get(&descriptor.server_instance_id())
      .cloned()
      .unwrap_or(ProbeOutcome::Unavailable)
    {
      ProbeOutcome::Ready => Ok(Handshake::new(
        descriptor.server_instance_id(),
        descriptor.scope_fingerprint().clone(),
      )),
      ProbeOutcome::Unavailable => Err(ProbeError::Unavailable),
      ProbeOutcome::AuthenticationRejected => Err(ProbeError::AuthenticationRejected),
      ProbeOutcome::WrongInstance => Ok(Handshake::new(
        Uuid::new_v4(),
        descriptor.scope_fingerprint().clone(),
      )),
    }
  }
}
