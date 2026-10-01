use std::sync::{
  Arc,
  atomic::{AtomicU64, Ordering},
};

use tokio::sync::mpsc;

#[derive(Clone)]
pub struct KernelControl {
  steering: mpsc::UnboundedSender<(u64, String)>,
  active_generation: Arc<AtomicU64>,
}

impl KernelControl {
  pub(crate) fn new(
    steering: mpsc::UnboundedSender<(u64, String)>,
    active_generation: Arc<AtomicU64>,
  ) -> Self {
    Self {
      steering,
      active_generation,
    }
  }

  pub fn steer(&self, text: impl Into<String>) -> Result<(), KernelControlError> {
    let text = text.into();
    if text.trim().is_empty() {
      return Err(KernelControlError::EmptySteering);
    }
    let generation = self.active_generation.load(Ordering::Acquire);
    if generation == 0 {
      return Err(KernelControlError::TurnNotRunning);
    }
    self
      .steering
      .send((generation, text))
      .map_err(|_| KernelControlError::TurnNotRunning)
  }
}

pub(crate) struct ActiveTurnGuard {
  active_generation: Arc<AtomicU64>,
  generation: u64,
}

impl ActiveTurnGuard {
  pub(crate) fn enter(active_generation: Arc<AtomicU64>, generation: u64) -> Self {
    active_generation.store(generation, Ordering::Release);
    Self {
      active_generation,
      generation,
    }
  }
}

impl Drop for ActiveTurnGuard {
  fn drop(&mut self) {
    let _ = self.active_generation.compare_exchange(
      self.generation,
      0,
      Ordering::AcqRel,
      Ordering::Acquire,
    );
  }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KernelControlError {
  #[error("steering text must not be empty")]
  EmptySteering,
  #[error("turn is not accepting steering input")]
  TurnNotRunning,
}
