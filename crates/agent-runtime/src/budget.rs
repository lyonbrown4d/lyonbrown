//! Hard host memory/persistence ceilings, independent of estimated model tokens.
use crate::{Result, RuntimeError};
use serde::Serialize;
use std::io::{self, Write};
pub(crate) const SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const CONTEXT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const HISTORY_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MODEL_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const TOOL_BYTES: usize = 16 * 1024 * 1024;

struct Counter {
  remaining: usize,
}
impl Write for Counter {
  fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
    self.remaining = self
      .remaining
      .checked_sub(bytes.len())
      .ok_or_else(|| io::Error::other("runtime state exceeds host byte limit"))?;
    Ok(bytes.len())
  }
  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}
pub(crate) fn size(value: &impl Serialize, limit: usize) -> Result<usize> {
  let mut counter = Counter { remaining: limit };
  serde_json::to_writer(&mut counter, value)
    .map_err(|_| RuntimeError::Configuration("runtime state exceeds host byte limit".into()))?;
  Ok(limit - counter.remaining)
}
pub(crate) fn check(value: &impl Serialize, limit: usize) -> Result<()> {
  size(value, limit).map(|_| ())
}
pub(crate) fn validate_snapshot(state: &crate::persistence::Snapshot) -> Result<()> {
  check(state, SNAPSHOT_BYTES)?;
  check(&state.context, CONTEXT_BYTES)?;
  if let Some(active) = &state.active {
    check(&active.run, HISTORY_BYTES)?;
  }
  Ok(())
}
static SERIALIZATION_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
pub(crate) async fn snapshot(
  state: crate::persistence::Snapshot,
) -> Result<(crate::persistence::Snapshot, Vec<u8>)> {
  let slot = SERIALIZATION_SLOTS
    .acquire()
    .await
    .map_err(|_| RuntimeError::Persistence("session serializer unavailable".into()))?;
  tokio::task::spawn_blocking(move || {
    let _slot = slot;
    validate_snapshot(&state)?;
    let bytes = serde_json::to_vec(&state)
      .map_err(|_| RuntimeError::Persistence("session serialization failed".into()))?;
    Ok((state, bytes))
  })
  .await
  .map_err(|_| RuntimeError::Persistence("session serializer failed".into()))?
}
